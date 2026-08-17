# RemoteBindingsExternalAccessRequest

## Example Usage

```typescript
import { RemoteBindingsExternalAccessRequest } from "@alienplatform/platform-api/models";

let value: RemoteBindingsExternalAccessRequest = {
  externalId: "ext_example_01",
  capability: "storage",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        | Example                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `externalId`                                                                                                       | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Case-sensitive, URL- and header-safe identifier from the integrating application.                                  | ext_example_01                                                                                                     |
| `capability`                                                                                                       | [models.RemoteBindingsExternalAccessRequestCapability](../models/remotebindingsexternalaccessrequestcapability.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |