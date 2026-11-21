# ExtensionBundleObject

## Example Usage

```typescript
import { ExtensionBundleObject } from "@alienplatform/platform-api/models";

let value: ExtensionBundleObject = {
  object: "<value>",
  sha256: "<value>",
  size: 232207,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `object`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | `core` for the core zip, or the controller binary's target, such as `linux-x64`. |
| `sha256`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | The object's sha256, as 64 lowercase hex characters.                             |
| `size`                                                                           | *number*                                                                         | :heavy_check_mark:                                                               | The object's size in bytes.                                                      |