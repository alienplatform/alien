# PutExternalAIBindingRequest

## Example Usage

```typescript
import { PutExternalAIBindingRequest } from "@alienplatform/platform-api/models";

let value: PutExternalAIBindingRequest = {
  provider: "anthropic",
  apiKey: "<value>",
  acknowledgeAlienCredentialAccess: true,
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `provider`                                                                                     | [models.PutExternalAIBindingRequestProvider](../models/putexternalaibindingrequestprovider.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `apiKey`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `acknowledgeAlienCredentialAccess`                                                             | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |