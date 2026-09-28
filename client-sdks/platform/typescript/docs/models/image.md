# Image

Image a running container reports.

## Example Usage

```typescript
import { Image } from "@alienplatform/platform-api/models";

let value: Image = {
  image: "https://loremflickr.com/1215/1658?lock=8122715376786738",
  name: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `digest`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Registry manifest digest in `sha256:<hex>` form, when the runtime<br/>reports one. |
| `image`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Image reference reported by the container runtime.                             |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | Container name.                                                                |