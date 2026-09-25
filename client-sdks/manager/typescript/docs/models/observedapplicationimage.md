# ObservedApplicationImage

A container image running in one observed application workload.

## Example Usage

```typescript
import { ObservedApplicationImage } from "@alienplatform/manager-api/models";

let value: ObservedApplicationImage = {
  container: "<value>",
  image: "https://loremflickr.com/2717/2282?lock=3275668383029292",
  workload: "<value>",
};
```

## Fields

| Field                                                                                                                                       | Type                                                                                                                                        | Required                                                                                                                                    | Description                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `container`                                                                                                                                 | *string*                                                                                                                                    | :heavy_check_mark:                                                                                                                          | Container name within the workload.                                                                                                         |
| `digest`                                                                                                                                    | *string*                                                                                                                                    | :heavy_minus_sign:                                                                                                                          | Registry manifest digest in `sha256:<hex>` form, when the runtime<br/>reports one.                                                          |
| `image`                                                                                                                                     | *string*                                                                                                                                    | :heavy_check_mark:                                                                                                                          | Image reference reported by the container runtime.                                                                                          |
| `workload`                                                                                                                                  | *string*                                                                                                                                    | :heavy_check_mark:                                                                                                                          | Inventory identity of the workload, matching the `rawIdentity` of its<br/>observed resource sample (for example `apps/v1:Deployment:shop:api`). |
