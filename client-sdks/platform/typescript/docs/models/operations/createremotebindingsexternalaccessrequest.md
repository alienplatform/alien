# CreateRemoteBindingsExternalAccessRequest

## Example Usage

```typescript
import { CreateRemoteBindingsExternalAccessRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateRemoteBindingsExternalAccessRequest = {
  idOrName: "<value>",
  remoteBindingsExternalAccessRequest: {
    externalId: "ext_example_01",
    capability: "sandbox",
  },
};
```

## Fields

| Field                                                                                             | Type                                                                                              | Required                                                                                          | Description                                                                                       |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `idOrName`                                                                                        | *string*                                                                                          | :heavy_check_mark:                                                                                | Project ID or name.                                                                               |
| `remoteBindingsExternalAccessRequest`                                                             | [models.RemoteBindingsExternalAccessRequest](../../models/remotebindingsexternalaccessrequest.md) | :heavy_check_mark:                                                                                | N/A                                                                                               |