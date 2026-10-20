# ReplicaRelease

The release a running replica's image belongs to.

## Example Usage

```typescript
import { ReplicaRelease } from "@alienplatform/platform-api/models/operations";

let value: ReplicaRelease = {
  replicaId: "<id>",
  image: "https://loremflickr.com/373/2242?lock=5332332070698998",
  release: {
    id: "<id>",
    version: "<value>",
    relation: "current",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `replicaId`                                                                                                    | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `image`                                                                                                        | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `release`                                                                                                      | [operations.GetResourceDeploymentDetailRelease](../../models/operations/getresourcedeploymentdetailrelease.md) | :heavy_check_mark:                                                                                             | The project release that ships this image for the resource, or null when none does.                            |