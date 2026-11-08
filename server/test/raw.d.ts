// Vite raw imports (test fixtures as text). Ambient: this file has no imports.
declare module "*?raw" {
  const text: string;
  export default text;
}
