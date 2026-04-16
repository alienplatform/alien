<!-- Start SDK Example Usage [usage] -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.health.health();

  console.log(result);
}

run();

```
<!-- End SDK Example Usage [usage] -->