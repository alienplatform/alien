# DeploymentStatePendingPreparedStackExtension

An extension a stack uses, and the resource types it defines.

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackExtension } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackExtension = {
  digest: "<value>",
  name: "<value>",
  resourceTypes: {
    "key": {
      controllerPlatforms: [
        "gcp",
      ],
      holdsData: true,
      lifecycles: [
        "frozen",
      ],
      link: {
        permissionSets: [],
      },
      permissionSets: [
        {
          description: "singing per mysteriously",
          id: "<id>",
          platforms: {},
        },
      ],
      schemas: {
        config: {
          "key": "<value>",
          "key1": "<value>",
          "key2": "<value>",
        },
        outputs: {
          "key": "<value>",
          "key1": "<value>",
          "key2": "<value>",
        },
      },
      setup: {
        cloudformation: [
          "test",
        ],
        helm: [],
        importers: [
          "test",
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
| `resourceTypes`                                                                                                                                                         | Record<string, [models.DeploymentStatePendingPreparedStackResourceTypes](../models/deploymentstatependingpreparedstackresourcetypes.md)>                                | :heavy_check_mark:                                                                                                                                                      | The resource types the extension defines, keyed by type name, such as "acme/dns-zone".                                                                                  |
| `version`                                                                                                                                                               | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | The extension's version.                                                                                                                                                |