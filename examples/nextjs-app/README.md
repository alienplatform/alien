# Next.js App

The smallest containerized Next.js app: one container, one page, one API route. Use it as the starting point for dashboards, internal tools, or any web app that needs to run where the customer's data lives.

The app builds with the included Dockerfile (Next.js [standalone output](https://nextjs.org/docs/app/api-reference/config/next-config-js/output)) and runs as a single replica behind an HTTPS load balancer.

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `app` | Container | The Next.js app, built from the Dockerfile and exposed over HTTP |

## Local development

Scaffold the template, then run the Next.js dev server directly:

```bash
alien init nextjs-app
cd nextjs-app

npm install
npm run dev
```

Then check it works:

```bash
curl http://localhost:3000/api/health
# {"status":"ok"}

open http://localhost:3000
```

## Deploying

```bash
alien deploy production --platform aws   # or gcp / azure
```

Alien builds the container image from the Dockerfile, pushes it, and provisions the compute and load balancer. The deploy output prints the public URL.

## Learn more

- [Quickstart guide](https://alien.dev/docs/quickstart) -- build a worker, test locally, send remote commands
- [How Alien Works](https://alien.dev/docs/how-alien-works) -- stacks, isolated areas, push vs pull
- [Stacks](https://alien.dev/docs/stacks) -- workers, storage, queues, vaults
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
