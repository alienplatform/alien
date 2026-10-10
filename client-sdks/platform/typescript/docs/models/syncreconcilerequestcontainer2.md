# SyncReconcileRequestContainer2

Image a running container reports.

## Example Usage

```typescript
import { SyncReconcileRequestContainer2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestContainer2 = {
  image: "https://loremflickr.com/1004/927?lock=8739425742154028",
  name: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `digest`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Registry manifest digest in `sha256:<hex>` form, when the runtime<br/>reports one. |
| `image`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Image reference reported by the container runtime.                             |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | Container name.                                                                |
