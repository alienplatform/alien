# ReleaseInfoLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { ReleaseInfoLifecycle } from "@alienplatform/platform-api/models";

let value: ReleaseInfoLifecycle = "live-on-setup";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```