# GetLiveDebugGrantRequest

## Example Usage

```typescript
import { GetLiveDebugGrantRequest } from "@alienplatform/platform-api/models/operations";

let value: GetLiveDebugGrantRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  debugTool: "kubectl",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                | Example                                                                    |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `deploymentId`                                                             | *string*                                                                   | :heavy_check_mark:                                                         | The deployment to check for a live debug grant.                            | dep_0c29fq4a2yjb7kx3smwdgxlc                                               |
| `debugTool`                                                                | [models.DebugGrantTool](../../models/debuggranttool.md)                    | :heavy_check_mark:                                                         | N/A                                                                        |                                                                            |
| `debugNamespace`                                                           | *string*                                                                   | :heavy_minus_sign:                                                         | Match a `kubectl` grant scoped to this namespace exactly.                  |                                                                            |
| `debugCloudScope`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | Match an `aws`/`gcloud`/`az` grant scoped to this account/project exactly. |                                                                            |