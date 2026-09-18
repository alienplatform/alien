# GetProjectActiveReleaseRequest

## Example Usage

```typescript
import { GetProjectActiveReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: GetProjectActiveReleaseRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `idOrName`                                         | *string*                                           | :heavy_check_mark:                                 | Project ID or name.                                |
| `deploymentId`                                     | *string*                                           | :heavy_minus_sign:                                 | Optional deployment ID to check for pinned release |