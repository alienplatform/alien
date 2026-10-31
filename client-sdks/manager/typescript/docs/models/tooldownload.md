# ToolDownload

A downloadable build of a tool.

## Example Usage

```typescript
import { ToolDownload } from "@alienplatform/manager-api/models";

let value: ToolDownload = {
  platform: "<value>",
  url: "https://clueless-bell.net/",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `platform`                                                             | *string*                                                               | :heavy_check_mark:                                                     | `<os>-<arch>`, e.g. `linux-x86_64`.                                    |
| `sha256`                                                               | *string*                                                               | :heavy_minus_sign:                                                     | SHA-256 of the exact downloaded bytes, when supplied by the publisher. |
| `url`                                                                  | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
