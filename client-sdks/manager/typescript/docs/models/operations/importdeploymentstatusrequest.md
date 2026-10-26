# ImportDeploymentStatusRequest

## Example Usage

```typescript
import { ImportDeploymentStatusRequest } from "@alienplatform/manager-api/models/operations";

let value: ImportDeploymentStatusRequest = {
  id: "<id>",
  statusReport: {
    heartbeats: [
      {
        backend: "kubernetes",
        controllerPlatform: "machines",
        data: {
          data: {
            adminUserEnabled: false,
            anonymousPullEnabled: false,
            dataEndpointHostNames: [],
            encryptionKeyIdentifierPresent: false,
            encryptionKeyVaultUriPresent: true,
            ipRuleCount: 303997,
            location: "<value>",
            managedTagCount: 444993,
            name: "<value>",
            networkRuleBypassOptions: "<value>",
            policiesPresent: false,
            policyCount: 431196,
            privateEndpointConnectionCount: 944892,
            publicNetworkAccess: "<value>",
            resourceGroup: "<value>",
            skuName: "<value>",
            status: {
              collectionIssues: [
                {
                  message: "<value>",
                  reason: "not-installed",
                  severity: "error",
                  source: "<value>",
                },
              ],
              health: "healthy",
              lifecycle: "scaling",
              partial: true,
              stale: false,
            },
            zoneRedundancy: "<value>",
            backend: "azureContainerRegistry",
          },
          resourceType: "artifact-registry",
        },
        observedAt: new Date("2024-03-31T04:44:14.623Z"),
        raw: [],
        resourceId: "<id>",
        resourceType: "worker",
      },
    ],
    state: {
      "key": "Indiana",
      "key1": "Virginia",
    },
  },
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `id`                                                | *string*                                            | :heavy_check_mark:                                  | Deployment ID                                       |
| `statusReport`                                      | [models.StatusReport](../../models/statusreport.md) | :heavy_check_mark:                                  | N/A                                                 |
