# CurrentReleaseOverrideAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { CurrentReleaseOverrideAzureBinding } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideAzureBinding = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `resource`                                                                                     | [models.CurrentReleaseOverrideAzureResource](../models/currentreleaseoverrideazureresource.md) | :heavy_minus_sign:                                                                             | Azure-specific binding specification                                                           |
| `stack`                                                                                        | [models.CurrentReleaseOverrideAzureStack](../models/currentreleaseoverrideazurestack.md)       | :heavy_minus_sign:                                                                             | Azure-specific binding specification                                                           |