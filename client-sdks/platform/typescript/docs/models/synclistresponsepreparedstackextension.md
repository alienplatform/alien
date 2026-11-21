# SyncListResponsePreparedStackExtension

An extension a stack uses, and the resource types it defines.

## Example Usage

```typescript
import { SyncListResponsePreparedStackExtension } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackExtension = {
  digest: "<value>",
  name: "<value>",
  resourceTypes: {
    "key": {
      controllerPlatforms: [
        "kubernetes",
      ],
      holdsData: false,
      lifecycles: [
        "frozen",
      ],
      link: {
        permissionSets: [
          "<value 1>",
          "<value 2>",
        ],
      },
      permissionSets: [
        {
          description:
            "supposing along irk crowded apropos extremely and wash vague certainly",
          id: "<id>",
          platforms: {},
        },
      ],
      schemas: {
        config: {},
        outputs: {},
      },
      setup: {
        cloudformation: [],
        helm: [],
        importers: [
          "gcp",
        ],
        terraform: [],
      },
    },
  },
  version: "<value>",
};
```

## Fields

| Field                                                                                                                                                                   | Type                                                                                                                                                                    | Required                                                                                                                                                                | Description                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `digest`                                                                                                                                                                | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | The sha256 digest of the extension's bundle. Every loader checks the bundle against it.                                                                                 |
| `egress`                                                                                                                                                                | *string*[]                                                                                                                                                              | :heavy_minus_sign:                                                                                                                                                      | The origins (`https://host[:port]`) the extension's controllers may call besides its<br/>cloud, as its bundle declares them. The setup template shows them to the customer. |
| `name`                                                                                                                                                                  | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | The extension's name, the first part of each type it defines, such as "acme".                                                                                           |
| `resourceTypes`                                                                                                                                                         | Record<string, [models.SyncListResponsePreparedStackResourceTypes](../models/synclistresponsepreparedstackresourcetypes.md)>                                            | :heavy_check_mark:                                                                                                                                                      | The resource types the extension defines, keyed by type name, such as "acme/dns-zone".                                                                                  |
| `version`                                                                                                                                                               | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | The extension's version.                                                                                                                                                |