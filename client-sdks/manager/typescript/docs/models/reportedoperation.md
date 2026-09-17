# ReportedOperation

A single operation the Operator currently has loaded. Opaque identifiers
only — no tier, description, or other plugin business logic crosses into
this public crate.

## Example Usage

```typescript
import { ReportedOperation } from "@alienplatform/manager-api/models";

let value: ReportedOperation = {
  name: "<value>",
  plugin: "<value>",
  pluginVersion: "<value>",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `name`                                          | *string*                                        | :heavy_check_mark:                              | Name of the operation within the plugin.        |
| `plugin`                                        | *string*                                        | :heavy_check_mark:                              | Name of the plugin that owns this operation.    |
| `pluginVersion`                                 | *string*                                        | :heavy_check_mark:                              | Version of the plugin that owns this operation. |