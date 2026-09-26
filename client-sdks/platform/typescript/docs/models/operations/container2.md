# Container2

Image a running container reports.

## Example Usage

```typescript
import { Container2 } from "@alienplatform/platform-api/models/operations";

let value: Container2 = {
  image: "https://loremflickr.com/762/2953?lock=683404111545208",
  name: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `digest`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Registry manifest digest in `sha256:<hex>` form, when the runtime<br/>reports one. |
| `image`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Image reference reported by the container runtime.                             |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | Container name.                                                                |