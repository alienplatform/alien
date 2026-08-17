# CurrentReleaseOverrideGcpBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { CurrentReleaseOverrideGcpBinding } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideGcpBinding = {};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `resource`                                                                                 | [models.CurrentReleaseOverrideGcpResource](../models/currentreleaseoverridegcpresource.md) | :heavy_minus_sign:                                                                         | GCP-specific binding specification                                                         |
| `stack`                                                                                    | [models.CurrentReleaseOverrideGcpStack](../models/currentreleaseoverridegcpstack.md)       | :heavy_minus_sign:                                                                         | GCP-specific binding specification                                                         |