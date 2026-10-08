# TargetDeploymentVolumeRestore

Replace one replica's persistent volume with a new volume made from a snapshot.

The controller stops the replica, snapshots the volume it is about to
replace (so the restore can be undone), creates the new volume in the same
zone, starts the replica on it, and deletes the replaced volume.

## Example Usage

```typescript
import { TargetDeploymentVolumeRestore } from "@alienplatform/platform-api/models";

let value: TargetDeploymentVolumeRestore = {
  ordinal: 869768,
  requestId: "<id>",
  resourceId: "<id>",
  snapshotId: "<id>",
};
```

## Fields

| Field                                                                                                                     | Type                                                                                                                      | Required                                                                                                                  | Description                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `ordinal`                                                                                                                 | *number*                                                                                                                  | :heavy_check_mark:                                                                                                        | Replica ordinal whose volume is replaced                                                                                  |
| `requestId`                                                                                                               | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | Unique ID of this request. A controller performs each request once.                                                       |
| `resourceId`                                                                                                              | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | ID of the container resource that owns the volume                                                                         |
| `snapshotId`                                                                                                              | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | Cloud ID of the snapshot to restore: an EBS snapshot ID, a Compute<br/>Engine snapshot name, or an Azure snapshot resource ID |