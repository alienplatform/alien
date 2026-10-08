# CreateVolumeRestoreRequest

## Example Usage

```typescript
import { CreateVolumeRestoreRequest } from "@alienplatform/platform-api/models";

let value: CreateVolumeRestoreRequest = {
  resourceId: "<id>",
  ordinal: 52769,
  snapshotId: "<id>",
};
```

## Fields

| Field                                                                                                                     | Type                                                                                                                      | Required                                                                                                                  | Description                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `resourceId`                                                                                                              | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | ID of the container resource that owns the volume                                                                         |
| `ordinal`                                                                                                                 | *number*                                                                                                                  | :heavy_check_mark:                                                                                                        | Replica ordinal whose volume is replaced                                                                                  |
| `snapshotId`                                                                                                              | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | Cloud ID of the snapshot to restore: an EBS snapshot ID, a Compute Engine snapshot name, or an Azure snapshot resource ID |
