// A test may import a table of cases shared with the Rust side, as a
// string: esbuild bundles the file's text (`--loader:.tsv=text`,
// tools/build_ui.py).
declare module "*.tsv" {
  const text: string;
  export default text;
}
