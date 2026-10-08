# ResolveRequest

## Example Usage

```typescript
import { ResolveRequest } from "@alienplatform/platform-api/models/operations";

let value: ResolveRequest = {
  platform: "local",
};
```

## Fields

| Field                                                                                                                                                         | Type                                                                                                                                                          | Required                                                                                                                                                      | Description                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                                                                    | [operations.ResolvePlatform](../../models/operations/resolveplatform.md)                                                                                      | :heavy_check_mark:                                                                                                                                            | Target platform to resolve the manager for                                                                                                                    |
| `project`                                                                                                                                                     | *string*                                                                                                                                                      | :heavy_minus_sign:                                                                                                                                            | Project ID or name. Required for user and workspace-scoped tokens. Optional for project/deployment-group/deployment-scoped tokens (derived from token scope). |