# StatusReport

## Example Usage

```typescript
import { StatusReport } from "@alienplatform/manager-api/models";

let value: StatusReport = {
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
    "key": "Iowa",
    "key1": "Indiana",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `appliedConfigurationRevision`                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | Approved configuration revision confirmed Running by the site.                                |
| `heartbeats`                                                                                  | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]                                  | :heavy_minus_sign:                                                                            | Resource observations captured by the Operator alongside this state.                          |
| `state`                                                                                       | Record<string, *any*>                                                                         | :heavy_check_mark:                                                                            | Deployment state as the environment's Operator last recorded it.                              |
| `telemetry`                                                                                   | [models.ReportedTelemetry](../models/reportedtelemetry.md)[]                                  | :heavy_minus_sign:                                                                            | Telemetry the Operator buffered, as the OTLP batches it received.                             |
| `writtenAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Site clock time when the report was written. Older clients omit it.                           |
