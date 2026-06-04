# SyncAcquireResponseDeleteResourceModeEnum

Resource set selected for deployment cleanup.

`All` is used for deployments where Alien owns the full recorded stack.
`Live` is used when setup tools own Frozen resources and Alien should only
delete resources it owns before setup tears down its part.

## Example Usage

```typescript
import { SyncAcquireResponseDeleteResourceModeEnum } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeleteResourceModeEnum = "live";
```

## Values

```typescript
"all" | "live"
```