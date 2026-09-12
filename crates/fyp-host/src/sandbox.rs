//! Loading and running a module in the WebAssembly sandbox (ADR 0003).
//!
//! One run, from the host's side:
//!
//! 1. The action, its parameters (checked against the manifest) and the
//!    documents are encoded ([`fyp_plugin_api::exchange`]) and become the
//!    module's standard input.
//! 2. A fresh store gets the manifest's limits: a memory ceiling enforced
//!    by a [`wasmtime::ResourceLimiter`] on every growth, a wall-clock
//!    deadline checked at every epoch tick, an output ceiling enforced on
//!    every write to standard output.
//! 3. `_start` runs on a dedicated thread while a second thread ticks the
//!    engine's epoch, so an endless loop is interrupted, never waited for.
//! 4. The answer is decoded. A document goes through [`revalidate`]: only
//!    the core's rewrite of it reaches the caller.
//!
//! The documents given to a run are borrowed read-only: whatever the
//! module does, they are untouched.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use fyp_core::document::Document;
use fyp_core::writer::{Writer, XrefStyle};
use fyp_plugin_api::exchange::{self, ParamValue, Response};
use fyp_plugin_api::{Action, Limits, Manifest, Permission, Runtime};
use wasmtime::{Config, Engine, ExternType, InstancePre, Module, Store, Trap, UpdateDeadline};

use crate::wasi::{self, Sandbox, Stop};
use crate::{DiscoveredModule, HostError};

/// File holding a module's code, next to its `manifest.toml`.
pub const MODULE_FILE: &str = "module.wasm";

const MIB: u64 = 1 << 20;
/// A wasm32 module addresses 4 GiB at most. Also the host's ceiling on
/// the answer it buffers, whatever a manifest says.
const WASM32_MAX: u64 = 4 << 30;
/// Room for the frame of an answer beyond the document itself.
const ANSWER_OVERHEAD: usize = 64 << 10;
/// Largest `module.wasm` the host compiles: compilation cannot be
/// interrupted, so its input is bounded instead.
const MAX_MODULE_BYTES: u64 = 64 << 20;
/// How often the epoch advances: the precision of the time limit.
const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Stack for WebAssembly frames; deeper recursion is a trap.
const MAX_WASM_STACK: usize = 2 << 20;
/// Native stack of the thread running a module: room for the WebAssembly
/// stack above plus the host frames around it.
const RUN_THREAD_STACK: usize = 16 << 20;

/// The WebAssembly engine, shared by the modules of a session.
#[derive(Clone)]
pub struct Host {
    engine: Engine,
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

impl Host {
    /// An engine configured for the sandbox: epoch interruption on, 32-bit
    /// memories only, bounded WebAssembly stack, deterministic floating
    /// point (NaN bits and relaxed SIMD would otherwise depend on the CPU).
    pub fn new() -> Result<Host, HostError> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        config.max_wasm_stack(MAX_WASM_STACK);
        config.wasm_memory64(false);
        config.cranelift_nan_canonicalization(true);
        config.relaxed_simd_deterministic(true);
        let engine = Engine::new(&config).map_err(|e| HostError::Engine(format!("{e:#}")))?;
        Ok(Host { engine })
    }

    /// Compile the `module.wasm` of a discovered module.
    pub fn load(&self, module: &DiscoveredModule) -> Result<LoadedModule, HostError> {
        let path = module.dir.join(MODULE_FILE);
        let io = |source| HostError::Io {
            path: path.clone(),
            source,
        };
        let mut wasm = Vec::new();
        File::open(&path)
            .map_err(io)?
            .take(MAX_MODULE_BYTES + 1)
            .read_to_end(&mut wasm)
            .map_err(io)?;
        self.load_bytes(module.manifest.clone(), &wasm)
    }

    /// Check `manifest`, compile `wasm` and link it. Refused here, before
    /// any of its code runs: a manifest that does not validate, a native
    /// runtime, a permission the host cannot grant yet, code that is not a
    /// WASI command (see [`HostError::BadModule`]).
    pub fn load_bytes(&self, manifest: Manifest, wasm: &[u8]) -> Result<LoadedModule, HostError> {
        manifest
            .validate(false)
            .map_err(|source| HostError::Manifest {
                path: "manifest.toml".into(),
                source,
            })?;
        let refuse = |message: String| HostError::BadModule {
            module: manifest.id.clone(),
            message,
        };
        if manifest.runtime != Runtime::Wasm {
            return Err(refuse(
                "declares the native runtime: the sandbox only runs WebAssembly".into(),
            ));
        }
        // Only the document permissions exist in this host. A module that
        // needs a directory, the network or a program would not work
        // without them: refusing it says so up front.
        if let Some(permission) = manifest
            .permissions
            .iter()
            .find(|p| !matches!(p, Permission::ReadDocument | Permission::WriteDocument))
        {
            return Err(HostError::PermissionUnavailable {
                module: manifest.id.clone(),
                permission: permission_name(permission),
            });
        }
        if u64::try_from(wasm.len()).map_or(true, |n| n > MAX_MODULE_BYTES) {
            return Err(refuse(format!(
                "{MODULE_FILE} is larger than the {} MiB the host compiles",
                MAX_MODULE_BYTES / MIB
            )));
        }
        let module = Module::from_binary(&self.engine, wasm)
            .map_err(|e| refuse(format!("not a valid WebAssembly module: {e:#}")))?;
        check_exports(&module).map_err(refuse)?;
        let linker = wasi::linker(&self.engine, &module).map_err(refuse)?;
        let pre = linker
            .instantiate_pre(&module)
            .map_err(|e| refuse(format!("{e:#}")))?;
        Ok(LoadedModule {
            manifest,
            engine: self.engine.clone(),
            pre,
        })
    }
}

/// A module ready to run: manifest checked, code compiled and linked.
pub struct LoadedModule {
    manifest: Manifest,
    engine: Engine,
    pre: InstancePre<Sandbox>,
}

impl fmt::Debug for LoadedModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedModule")
            .field("id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .finish_non_exhaustive()
    }
}

/// What a successful run gives back.
#[derive(Debug, Clone)]
pub struct RunOutput {
    /// The returned document after [`revalidate`]: the core's rewrite, not
    /// the module's bytes.
    pub document: Vec<u8>,
    /// Why the module's document needed a reconstruction before it could
    /// be rewritten, when it did.
    pub reconstructed: Option<fyp_core::Error>,
    /// What the module wrote on its standard error (first 64 KiB).
    pub diagnostics: String,
}

/// What the store holds when `_start` is over.
struct Finished {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit: Option<i32>,
}

/// The manifest's limits in the units the runtime counts.
struct Budget {
    timeout: Duration,
    memory: usize,
    output: usize,
}

impl Budget {
    fn of(limits: &Limits) -> Budget {
        let bytes = |mib: u64| {
            usize::try_from(mib.saturating_mul(MIB).min(WASM32_MAX)).unwrap_or(usize::MAX)
        };
        Budget {
            timeout: Duration::from_millis(limits.timeout_ms),
            memory: bytes(limits.memory_mib),
            output: bytes(limits.max_output_mib),
        }
    }
}

impl LoadedModule {
    /// The module's manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Run `action` on `documents` with `params`. Everything the manifest
    /// declares is checked before the module starts (action, number of
    /// documents, parameters, `read_document`); the limits apply while it
    /// runs; the returned document is re-validated after.
    pub fn run(
        &self,
        action: &str,
        params: &BTreeMap<String, ParamValue>,
        documents: &[&[u8]],
    ) -> Result<RunOutput, HostError> {
        let module = || self.manifest.id.clone();
        let spec = self
            .manifest
            .actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| HostError::UnknownAction {
                module: module(),
                action: action.to_string(),
            })?;
        if usize::try_from(spec.min_inputs).map_or(true, |min| documents.len() < min) {
            return Err(HostError::NotEnoughInputs {
                module: module(),
                action: action.to_string(),
                min: spec.min_inputs,
                given: documents.len(),
            });
        }
        check_params(spec, params).map_err(|message| HostError::BadParameter {
            module: module(),
            action: action.to_string(),
            message,
        })?;
        if !documents.is_empty() {
            self.require(&Permission::ReadDocument)?;
        }
        let request = exchange::encode_request(action, params, documents)
            .map_err(|e| HostError::Internal(format!("cannot encode the request: {e}")))?;
        let budget = Budget::of(&self.manifest.limits);
        let finished = self.execute(request, &budget)?;
        let diagnostics = String::from_utf8_lossy(&finished.stderr).into_owned();
        let answer = Response::decode(&finished.stdout);
        match (answer, finished.exit.filter(|&code| code != 0)) {
            (Ok(Response::Error(message)), _) => Err(HostError::ModuleFailed {
                module: module(),
                message,
            }),
            (Ok(Response::Document(bytes)), None) => self.accept(&bytes, &budget, diagnostics),
            (_, Some(code)) => Err(HostError::Exited {
                module: module(),
                code,
                diagnostics,
            }),
            (Err(_), None) if finished.stdout.is_empty() => Err(HostError::BadResponse {
                module: module(),
                message: "ended without writing an answer".into(),
                diagnostics,
            }),
            (Err(e), None) => Err(HostError::BadResponse {
                module: module(),
                message: format!("unreadable answer: {e}"),
                diagnostics,
            }),
        }
    }

    fn require(&self, permission: &Permission) -> Result<(), HostError> {
        if self.manifest.permissions.contains(permission) {
            Ok(())
        } else {
            Err(HostError::PermissionNotDeclared {
                module: self.manifest.id.clone(),
                permission: permission_name(permission),
            })
        }
    }

    fn accept(
        &self,
        bytes: &[u8],
        budget: &Budget,
        diagnostics: String,
    ) -> Result<RunOutput, HostError> {
        self.require(&Permission::WriteDocument)?;
        if bytes.len() > budget.output {
            return Err(HostError::OutputTooLarge {
                module: self.manifest.id.clone(),
                limit_mib: self.manifest.limits.max_output_mib,
            });
        }
        let (document, reconstructed) =
            revalidate(bytes).map_err(|reason| HostError::Rejected {
                module: self.manifest.id.clone(),
                reason,
            })?;
        Ok(RunOutput {
            document,
            reconstructed,
            diagnostics,
        })
    }

    /// Run `_start` on its own thread, with the epoch ticker beside it.
    fn execute(&self, stdin: Vec<u8>, budget: &Budget) -> Result<Finished, HostError> {
        let (stop_ticker, ticks) = mpsc::channel::<()>();
        thread::scope(|scope| {
            let engine = self.engine.clone();
            let ticker = thread::Builder::new()
                .name("fyp-module-epoch".into())
                .spawn_scoped(scope, move || {
                    while let Err(RecvTimeoutError::Timeout) = ticks.recv_timeout(EPOCH_TICK) {
                        engine.increment_epoch();
                    }
                })
                .map_err(|e| HostError::Internal(format!("cannot start the epoch thread: {e}")))?;
            let result = thread::Builder::new()
                .name("fyp-module".into())
                .stack_size(RUN_THREAD_STACK)
                .spawn_scoped(scope, move || self.start(stdin, budget))
                .map_err(|e| HostError::Internal(format!("cannot start the module thread: {e}")))
                .and_then(|worker| {
                    worker.join().unwrap_or_else(|_| {
                        Err(HostError::Internal("the module thread panicked".into()))
                    })
                });
            let _ = stop_ticker.send(());
            let _ = ticker.join();
            result
        })
    }

    fn start(&self, stdin: Vec<u8>, budget: &Budget) -> Result<Finished, HostError> {
        let mut store = Store::new(
            &self.engine,
            Sandbox::new(
                stdin,
                budget.output.saturating_add(ANSWER_OVERHEAD),
                budget.memory,
            ),
        );
        store.limiter(|sandbox| sandbox);
        // The deadline is checked at each tick rather than set as a number
        // of ticks: other runs on the same engine tick it too.
        let (started, timeout) = (Instant::now(), budget.timeout);
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(move |_| {
            if started.elapsed() >= timeout {
                Err(wasmtime::Error::new(Stop::Timeout))
            } else {
                Ok(UpdateDeadline::Continue(1))
            }
        });
        let outcome = match self.pre.instantiate(&mut store) {
            Ok(instance) => match instance.get_typed_func::<(), ()>(&mut store, "_start") {
                Ok(start) => start.call(&mut store, ()),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let exit = match outcome {
            Ok(()) => None,
            Err(error) => match error.downcast_ref::<Stop>() {
                Some(Stop::Exit(code)) => Some(*code),
                _ => return Err(self.classify(&error, &store.data().stderr)),
            },
        };
        let sandbox = store.into_data();
        Ok(Finished {
            stdout: sandbox.stdout,
            stderr: sandbox.stderr,
            exit,
        })
    }

    fn classify(&self, error: &wasmtime::Error, stderr: &[u8]) -> HostError {
        let module = self.manifest.id.clone();
        let limits = &self.manifest.limits;
        let diagnostics = String::from_utf8_lossy(stderr).into_owned();
        match error.downcast_ref::<Stop>() {
            Some(Stop::Timeout) => HostError::Timeout {
                module,
                limit_ms: limits.timeout_ms,
            },
            Some(Stop::Memory) => HostError::MemoryExceeded {
                module,
                limit_mib: limits.memory_mib,
            },
            Some(Stop::Output) => HostError::OutputTooLarge {
                module,
                limit_mib: limits.max_output_mib,
            },
            Some(Stop::Denied(import)) => HostError::CapabilityDenied {
                module,
                import: import.clone(),
            },
            Some(Stop::NoMemory) => HostError::BadModule {
                module,
                message: "exports no memory".into(),
            },
            Some(Stop::Exit(code)) => HostError::Exited {
                module,
                code: *code,
                diagnostics,
            },
            None => HostError::Trapped {
                module,
                message: match error.downcast_ref::<Trap>() {
                    Some(trap) => trap.to_string(),
                    None => format!("{error:#}"),
                },
                diagnostics,
            },
        }
    }
}

/// The host's gate on every document a module returns (ADR 0003, point 3).
///
/// The bytes are opened by [`Document::open`], which reconstructs the
/// cross-reference table when it is unusable, then rewritten by the core's
/// writer. Only that rewrite is handed on: an object the parser cannot
/// read, a stray byte, an extra update section never survive it. The
/// rewrite must reopen without repair and have at least one page.
///
/// Returns the rewritten bytes and, when the given document needed a
/// reconstruction, why.
pub fn revalidate(bytes: &[u8]) -> Result<(Vec<u8>, Option<fyp_core::Error>), fyp_core::Error> {
    let doc = Document::open(bytes)?;
    has_pages(&doc)?;
    let version = doc.version();
    // A numbering too sparse for a classic table still has a conformant
    // form: a cross-reference stream.
    let written = match Writer::new(version).write(&doc) {
        Ok(written) => written,
        Err(first) => Writer::new(version)
            .xref_style(XrefStyle::Stream)
            .write(&doc)
            .map_err(|_| first)?,
    };
    let check = Document::open(&written)?;
    if let Some(reason) = check.reconstructed() {
        return Err(fyp_core::Error::BadStructure {
            message: format!("the rewritten document needs repair: {reason}"),
        });
    }
    has_pages(&check)?;
    Ok((written, doc.reconstructed().cloned()))
}

fn has_pages(doc: &Document<'_>) -> fyp_core::Result<()> {
    match doc.page_count()? {
        0 => Err(fyp_core::Error::BadStructure {
            message: "the document has no page".into(),
        }),
        _ => Ok(()),
    }
}

/// A WASI command: `_start` taking and returning nothing, one exported
/// memory, not shared (the memory limiter does not see shared memories).
fn check_exports(module: &Module) -> Result<(), String> {
    let mut start = false;
    let mut memory = false;
    for export in module.exports() {
        match (export.name(), export.ty()) {
            ("_start", ExternType::Func(ty)) => {
                start = ty.params().next().is_none() && ty.results().next().is_none();
            }
            ("memory", ExternType::Memory(ty)) => {
                if ty.is_shared() {
                    return Err("exports a shared memory, which the sandbox refuses".into());
                }
                memory = true;
            }
            _ => {}
        }
    }
    match (start, memory) {
        (true, true) => Ok(()),
        (false, _) => Err("no `_start() -> ()` export: not a WASI command".into()),
        (true, false) => Err("no `memory` export".into()),
    }
}

/// Every parameter given is declared, of its kind, within its bounds, and
/// every required one is given.
fn check_params(action: &Action, params: &BTreeMap<String, ParamValue>) -> Result<(), String> {
    for (name, value) in params {
        let Some(spec) = action.params.iter().find(|p| &p.id == name) else {
            return Err(format!("unknown parameter `{name}`"));
        };
        if value.kind() != spec.kind {
            return Err(format!(
                "parameter `{name}` is {}, not {}",
                spec.kind,
                value.kind()
            ));
        }
        if let ParamValue::Integer(n) = value {
            if spec.min.is_some_and(|min| *n < min) || spec.max.is_some_and(|max| *n > max) {
                return Err(format!("parameter `{name}` = {n} is out of its bounds"));
            }
        }
    }
    match action
        .params
        .iter()
        .find(|p| p.required && !params.contains_key(&p.id))
    {
        Some(missing) => Err(format!("missing required parameter `{}`", missing.id)),
        None => Ok(()),
    }
}

/// A permission as manifests spell it.
fn permission_name(permission: &Permission) -> String {
    match permission {
        Permission::ReadDocument => "read_document".into(),
        Permission::WriteDocument => "write_document".into(),
        Permission::ReadDir => "read_dir".into(),
        Permission::WriteDir => "write_dir".into(),
        Permission::Network { hosts } => format!("network ({})", hosts.join(", ")),
        Permission::Subprocess { program } => format!("subprocess ({program})"),
    }
}
