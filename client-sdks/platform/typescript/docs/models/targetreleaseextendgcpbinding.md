# TargetReleaseExtendGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseExtendGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendGcpBinding = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `resource`                                                                           | [models.TargetReleaseExtendGcpResource](../models/targetreleaseextendgcpresource.md) | :heavy_minus_sign:                                                                   | GCP-specific binding specification                                                   |
| `stack`                                                                              | [models.TargetReleaseExtendGcpStack](../models/targetreleaseextendgcpstack.md)       | :heavy_minus_sign:                                                                   | GCP-specific binding specification                                                   |