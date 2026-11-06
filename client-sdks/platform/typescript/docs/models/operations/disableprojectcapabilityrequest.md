# DisableProjectCapabilityRequest

## Example Usage

```typescript
import { DisableProjectCapabilityRequest } from "@alienplatform/platform-api/models/operations";

let value: DisableProjectCapabilityRequest = {
  idOrName: "<value>",
  capability: "remote-sandbox",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `idOrName`                                                     | *string*                                                       | :heavy_check_mark:                                             | Project ID or name.                                            |
| `capability`                                                   | [operations.Capability](../../models/operations/capability.md) | :heavy_check_mark:                                             | N/A                                                            |