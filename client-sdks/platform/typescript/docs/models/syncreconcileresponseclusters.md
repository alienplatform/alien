# SyncReconcileResponseClusters

Configuration for a single Horizon cluster.

Contains the cluster ID and management token needed to interact with
the Horizon control plane API for container operations.

## Example Usage

```typescript
import { SyncReconcileResponseClusters } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseClusters = {
  clusterId: "<id>",
  managementToken: "<value>",
};
```

## Fields

| Field                                                                                                     | Type                                                                                                      | Required                                                                                                  | Description                                                                                               |
| --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `clusterId`                                                                                               | *string*                                                                                                  | :heavy_check_mark:                                                                                        | Cluster ID (deterministic: workspace/project/agent/resourceid)                                            |
| `managementToken`                                                                                         | *string*                                                                                                  | :heavy_check_mark:                                                                                        | Management token for API access (hm_...)<br/>Used by alien-deployment controllers to create/update containers |