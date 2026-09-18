# ConfigureProjectSourceModels

## Example Usage

```typescript
import { ConfigureProjectSourceModels } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceModels = {
  enabled: true,
  allowedProviders: [],
  requirements: [],
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                              | *boolean*                                                                                                              | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `allowedProviders`                                                                                                     | [operations.ConfigureProjectSourceAllowedProvider](../../models/operations/configureprojectsourceallowedprovider.md)[] | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `requirements`                                                                                                         | [operations.ConfigureProjectSourceRequirement](../../models/operations/configureprojectsourcerequirement.md)[]         | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |