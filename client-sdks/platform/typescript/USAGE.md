<!-- Start SDK Example Usage [usage] -->
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile();

  console.log(result);
}

run();

```
<!-- End SDK Example Usage [usage] -->