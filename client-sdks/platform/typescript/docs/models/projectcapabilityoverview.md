# ProjectCapabilityOverview

## Example Usage

```typescript
import { ProjectCapabilityOverview } from "@alienplatform/platform-api/models";

let value: ProjectCapabilityOverview = {
  generatedAt: new Date("2024-09-06T15:21:08.715Z"),
  configurationStatus: "valid",
  summary: {
    groups: 180460,
    capabilities: {
      models: {
        enabled: false,
        connected: 125192,
        settingUp: 258850,
        needsAttention: 397545,
        revoked: 804965,
        notConnected: 379194,
      },
      keys: {
        enabled: true,
        connected: 320779,
        settingUp: 72958,
        needsAttention: 143069,
        revoked: 714520,
        notConnected: 306654,
      },
      buckets: {
        enabled: false,
        connected: 546814,
        settingUp: 839265,
        needsAttention: 603306,
        revoked: 730330,
        notConnected: 924949,
      },
      registry: {
        enabled: true,
        connected: 336407,
        settingUp: 784831,
        needsAttention: 961354,
        revoked: 697113,
        notConnected: 723984,
      },
      remoteSandbox: {
        enabled: false,
        connected: 965448,
        settingUp: 892003,
        needsAttention: 68118,
        revoked: 27267,
        notConnected: 127508,
      },
    },
  },
  groups: [
    {
      deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
      name: "<value>",
      externalId: "<id>",
      createdAt: new Date("2026-02-20T18:56:11.695Z"),
      capabilities: {
        models: {
          capability: "remoteSandbox",
          enabled: true,
          state: "not-connected",
          deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
          observation: {
            provider: "<value>",
            platform: "local",
            resourceId: "<id>",
            location: "<value>",
            account: "22393779",
            observedAt: new Date("2025-02-05T09:16:14.437Z"),
            stale: false,
            partial: true,
            health: null,
            lifecycle: "<value>",
            message: "<value>",
          },
        },
        keys: {
          capability: "registry",
          enabled: true,
          state: "connected",
          deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
          observation: {
            provider: "<value>",
            platform: "gcp",
            resourceId: "<id>",
            location: "<value>",
            account: "19573632",
            observedAt: new Date("2024-06-20T03:52:32.683Z"),
            stale: true,
            partial: true,
            health: "degraded",
            lifecycle: "<value>",
            message: "<value>",
          },
        },
        buckets: {
          capability: "models",
          enabled: true,
          state: "needs-attention",
          deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
          observation: {
            provider: "<value>",
            platform: "test",
            resourceId: "<id>",
            location: "<value>",
            account: "58292455",
            observedAt: new Date("2024-09-29T01:47:42.152Z"),
            stale: false,
            partial: false,
            health: "unknown",
            lifecycle: "<value>",
            message: "<value>",
          },
        },
        registry: {
          capability: "buckets",
          enabled: false,
          state: "needs-attention",
          deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
          observation: {
            provider: "<value>",
            platform: "gcp",
            resourceId: "<id>",
            location: "<value>",
            account: "07843950",
            observedAt: new Date("2026-07-02T12:17:56.583Z"),
            stale: true,
            partial: true,
            health: "unknown",
            lifecycle: null,
            message: "<value>",
          },
        },
        remoteSandbox: {
          capability: "buckets",
          enabled: false,
          state: "not-connected",
          deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
          observation: {
            provider: "<value>",
            platform: "gcp",
            resourceId: "<id>",
            location: "<value>",
            account: null,
            observedAt: new Date("2025-11-23T13:41:17.370Z"),
            stale: true,
            partial: true,
            health: "healthy",
            lifecycle: "<value>",
            message: "<value>",
          },
        },
      },
    },
  ],
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `generatedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `configurationStatus`                                                                         | [models.ConfigurationStatus](../models/configurationstatus.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `summary`                                                                                     | [models.ProjectCapabilityOverviewSummary](../models/projectcapabilityoverviewsummary.md)      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `groups`                                                                                      | [models.Group](../models/group.md)[]                                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |