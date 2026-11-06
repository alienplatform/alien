# DeploymentInfo

## Example Usage

```typescript
import { DeploymentInfo } from "@alienplatform/platform-api/models";

let value: DeploymentInfo = {
  tokenType: "deployment-group",
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
    name: "<value>",
    pinnedSubdomain: "<value>",
    maxDeployments: 737152,
    currentCount: 255496,
    lastDeletedDeployment: null,
  },
  workspace: {
    id: "<id>",
    name: "<value>",
  },
  project: {
    name: "<value>",
    portal: {
      appearance: {},
    },
  },
  packages: {
    ready: true,
  },
  installContext: {
    targets: {
      "key": {
        platform: "gcp",
        managerUrl: "https://unsightly-supplier.info",
      },
    },
  },
  supportedRegions: {
    aws: [],
    gcp: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    azure: [
      "<value 1>",
      "<value 2>",
    ],
  },
  modelAvailabilitySources: [],
  setupItems: [
    {
      item: "deployment",
      source: {
        type: "built-in",
        definitionId: "customer-ai",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      },
      required: false,
      status: "not-started",
      deploymentIds: [
        "<value 1>",
      ],
    },
  ],
  setupConfig: {
    metadata: {},
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [],
    },
    environmentVariables: [
      {
        name: "<value>",
        type: "secret",
        targetResources: null,
      },
    ],
    items: [
      {
        item: "deployment",
        source: {
          type: "project-release",
          releaseChannel: "<value>",
          releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        },
        required: false,
      },
    ],
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `setupUpdate`                                                                                                  | [models.SetupUpdate](../models/setupupdate.md)                                                                 | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `tokenType`                                                                                                    | [models.DeploymentInfoTokenType](../models/deploymentinfotokentype.md)                                         | :heavy_check_mark:                                                                                             | Type of token used to authenticate this request                                                                |
| `deployment`                                                                                                   | [models.DeploymentInfoDeployment](../models/deploymentinfodeployment.md)                                       | :heavy_minus_sign:                                                                                             | Deployment details (present when using a deployment-scoped token)                                              |
| `deploymentGroup`                                                                                              | [models.DeploymentInfoDeploymentGroup](../models/deploymentinfodeploymentgroup.md)                             | :heavy_minus_sign:                                                                                             | Deployment group details (present when using a deployment-group token)                                         |
| `workspace`                                                                                                    | [models.DeploymentInfoWorkspace](../models/deploymentinfoworkspace.md)                                         | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `project`                                                                                                      | [models.DeploymentInfoProject](../models/deploymentinfoproject.md)                                             | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `packages`                                                                                                     | [models.Packages](../models/packages.md)                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `installContext`                                                                                               | [models.DeploymentInfoInstallContext](../models/deploymentinfoinstallcontext.md)                               | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `supportedRegions`                                                                                             | [models.SupportedCloudRegions](../models/supportedcloudregions.md)                                             | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `modelAvailabilitySources`                                                                                     | [models.ModelAvailabilitySource](../models/modelavailabilitysource.md)[]                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `airgappedAvailableOnPlan`                                                                                     | *boolean*                                                                                                      | :heavy_minus_sign:                                                                                             | Whether the workspace plan permits air-gapped delivery. Link policy and Kubernetes support must also allow it. |
| `setupItems`                                                                                                   | [models.SetupItemStatus](../models/setupitemstatus.md)[]                                                       | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `setupConfig`                                                                                                  | [models.DeploymentInfoSetupConfig](../models/deploymentinfosetupconfig.md)                                     | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `readiness`                                                                                                    | [models.Readiness](../models/readiness.md)                                                                     | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |