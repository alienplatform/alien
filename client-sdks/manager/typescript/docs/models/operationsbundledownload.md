# OperationsBundleDownload

One bundle the Operator needs to download to reach `targetBundleHash`.
The manager mints a short-lived presigned GET URL per bundle — the
Operator never holds real cloud storage credentials, mirroring the OCI
registry proxy's credential-injection pattern.

## Example Usage

```typescript
import { OperationsBundleDownload } from "@alienplatform/manager-api/models";

let value: OperationsBundleDownload = {
  plugin: "<value>",
  pluginVersion: "<value>",
  url: "https://those-mobility.name",
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `env`                                                                                                                                    | Record<string, *string*>                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Environment the plugin process runs with: its settings, which may hold<br/>secrets. Store only in encrypted state, and never log the values. |
| `plugin`                                                                                                                                 | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Plugin name this bundle provides.                                                                                                        |
| `pluginVersion`                                                                                                                          | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Plugin version this bundle provides.                                                                                                     |
| `url`                                                                                                                                    | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Presigned URL to GET the bundle ZIP from. Short-lived.                                                                                   |
