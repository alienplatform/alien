# Resource


## Supported Types

### `operations.ResourceManaged`

```typescript
const value: operations.ResourceManaged = {
  resourceType: "<value>",
  resourceId: "<id>",
  name: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  health: "<value>",
  lifecycle: "<value>",
  message: null,
  partial: true,
  providerStale: false,
  platformStale: false,
  desiredCount: 364799,
  currentCount: 399507,
  readyCount: 342238,
  deploymentCount: 524553,
  attentionCount: 468100,
  lastObservedAt: new Date("2026-02-15T18:26:49.854Z"),
  source: "managed",
  deploymentId: "<id>",
  deploymentName: "<value>",
};
```
### `operations.ResourceObserved`

```typescript
const value: operations.ResourceObserved = {
  source: "observed",
  deploymentId: "<id>",
  deploymentName: "<value>",
  deploymentGroupId: "<id>",
  deploymentGroupName: "<value>",
  resourceType: "<value>",
  resourceId: "<id>",
  name: "<value>",
  rawKind: "<value>",
  alienResourceId: null,
  backend: "<value>",
  controllerPlatform: "<value>",
  health: "<value>",
  lifecycle: "<value>",
  message: "<value>",
  partial: false,
  providerStale: false,
  platformStale: false,
  desiredCount: null,
  currentCount: null,
  readyCount: 801163,
  deploymentCount: 475958,
  attentionCount: 570932,
  lastObservedAt: new Date("2025-12-16T15:58:04.032Z"),
};
```
