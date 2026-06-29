export default function Home() {
  return (
    <main style={{ fontFamily: "sans-serif", maxWidth: "40rem", margin: "4rem auto" }}>
      <h1>Next.js on Alien</h1>
      <p>
        This app runs as a single container inside the cloud account it was deployed to. Edit{" "}
        <code>app/page.tsx</code> and redeploy to ship changes.
      </p>
      <p>
        Try the API route at <a href="/api/health">/api/health</a>.
      </p>
    </main>
  )
}
