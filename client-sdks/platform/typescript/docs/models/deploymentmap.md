# DeploymentMap

## Example Usage

```typescript
import { DeploymentMap } from "@alienplatform/platform-api/models";

let value: DeploymentMap = {
  items: [
    {
      id: "<id>",
      name: "<value>",
      deploymentGroupId: "<id>",
      deploymentGroupName: "<value>",
      purpose: "encryption",
      mapState: "stopped",
      releaseChannel: "<value>",
      pinned: true,
      reporting: true,
      onRelease: true,
      released: true,
      status: "running",
      setupProgress: "awaiting",
      platform: "<value>",
      region: "<value>",
      installedVia: "setup-link",
      releaseCommitSha: "<value>",
      changedAt: "<value>",
    },
  ],
  total: 2352.57,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `items`                                                      | [models.DeploymentMapItem](../models/deploymentmapitem.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `total`                                                      | *number*                                                     | :heavy_check_mark:                                           | All deployments of the project, even past the cap            |