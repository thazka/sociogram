import { GraphDemo } from "./graph-demo";

// A Server Component: it is rendered on the server, and only the client
// component below ever touches the browser.
export default function Page() {
  return (
    <main>
      <h1>sociogram + Next.js</h1>
      <GraphDemo />
    </main>
  );
}
