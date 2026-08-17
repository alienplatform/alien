# TargetReleaseOverrideGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseOverrideGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverrideGcpBinding = {};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `resource`                                                                               | [models.TargetReleaseOverrideGcpResource](../models/targetreleaseoverridegcpresource.md) | :heavy_minus_sign:                                                                       | GCP-specific binding specification                                                       |
| `stack`                                                                                  | [models.TargetReleaseOverrideGcpStack](../models/targetreleaseoverridegcpstack.md)       | :heavy_minus_sign:                                                                       | GCP-specific binding specification                                                       |