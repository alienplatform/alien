# DeploymentAirgappedBundleDownloadedEvent

## Example Usage

```typescript
import { DeploymentAirgappedBundleDownloadedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentAirgappedBundleDownloadedEvent = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  downloadedAt: new Date("2025-07-23T22:38:40.273Z"),
  type: "DeploymentAirgappedBundleDownloaded",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `releaseId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9E2Su                                                              |
| `downloadedAt`                                                                                | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `type`                                                                                        | *"DeploymentAirgappedBundleDownloaded"*                                                       | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                              |