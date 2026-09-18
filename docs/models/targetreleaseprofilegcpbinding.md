# TargetReleaseProfileGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseProfileGcpBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileGcpBinding = {};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `resource`                                                                             | [models.TargetReleaseProfileGcpResource](../models/targetreleaseprofilegcpresource.md) | :heavy_minus_sign:                                                                     | GCP-specific binding specification                                                     |
| `stack`                                                                                | [models.TargetReleaseProfileGcpStack](../models/targetreleaseprofilegcpstack.md)       | :heavy_minus_sign:                                                                     | GCP-specific binding specification                                                     |