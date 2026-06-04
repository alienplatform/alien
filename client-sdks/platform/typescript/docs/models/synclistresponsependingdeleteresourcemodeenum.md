# SyncListResponsePendingDeleteResourceModeEnum

Resource set selected for deployment cleanup.

`All` is used for deployments where Alien owns the full recorded stack.
`Live` is used when setup tools own Frozen resources and Alien should only
delete resources it owns before setup tears down its part.

## Example Usage

```typescript
import { SyncListResponsePendingDeleteResourceModeEnum } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingDeleteResourceModeEnum = "all";
```

## Values

```typescript
"all" | "live"
```