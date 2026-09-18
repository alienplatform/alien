# ResolveCommandTargetRequest

## Example Usage

```typescript
import { ResolveCommandTargetRequest } from "@alienplatform/platform-api/models/operations";

let value: ResolveCommandTargetRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         | Example                                                             |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `deploymentId`                                                      | *string*                                                            | :heavy_check_mark:                                                  | Deployment to resolve the target for                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                        |
| `target`                                                            | *string*                                                            | :heavy_minus_sign:                                                  | Explicit resource id to resolve; must be a command-capable resource |                                                                     |