# Group

## Example Usage

```typescript
import { Group } from "@alienplatform/platform-api/models";

let value: Group = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  name: "<value>",
  externalId: "<id>",
  createdAt: new Date("2024-04-05T05:54:15.339Z"),
  capabilities: {
    models: {
      capability: "registry",
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
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment group.                                                   | dg_r27ict8c7vcgsumpj90ackf7b                                                                  |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `externalId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `capabilities`                                                                                | [models.GroupCapabilities](../models/groupcapabilities.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |