# PackageTokenSecret

Non-secret configuration for a chart-generated enrollment token.

## Example Usage

```typescript
import { PackageTokenSecret } from "@alienplatform/platform-api/models";

let value: PackageTokenSecret = {
  key: "<key>",
  name: "<value>",
  prefix: "<value>",
  randomLength: 329334,
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `key`                                                                 | *string*                                                              | :heavy_check_mark:                                                    | Data key inside the Secret.                                           |
| `name`                                                                | *string*                                                              | :heavy_check_mark:                                                    | Fixed Kubernetes Secret name expected by the workload's Secret mount. |
| `prefix`                                                              | *string*                                                              | :heavy_check_mark:                                                    | Plaintext prefix before the random alphanumeric suffix.               |
| `randomLength`                                                        | *number*                                                              | :heavy_check_mark:                                                    | Length of the random suffix.                                          |
