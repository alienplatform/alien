# OverrideReleaseInfoGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { OverrideReleaseInfoGcpBinding } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoGcpBinding = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `resource`                                                                           | [models.OverrideReleaseInfoGcpResource](../models/overridereleaseinfogcpresource.md) | :heavy_minus_sign:                                                                   | GCP-specific binding specification                                                   |
| `stack`                                                                              | [models.OverrideReleaseInfoGcpStack](../models/overridereleaseinfogcpstack.md)       | :heavy_minus_sign:                                                                   | GCP-specific binding specification                                                   |