# SetupLinkEntryPoint

Portal destination to open first. This controls navigation, not authorization.

## Example Usage

```typescript
import { SetupLinkEntryPoint } from "@alienplatform/platform-api/models";

let value: SetupLinkEntryPoint = {
  item: "keys",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `item`                                                                 | [models.SetupLinkEntryPointItem](../models/setuplinkentrypointitem.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `provider`                                                             | *models.SetupLinkEntryPointProviderUnion*                              | :heavy_minus_sign:                                                     | N/A                                                                    |