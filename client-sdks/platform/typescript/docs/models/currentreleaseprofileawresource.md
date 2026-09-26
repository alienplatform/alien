# CurrentReleaseProfileAwResource

AWS-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseProfileAwResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseProfileAwResource = {
  resources: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                                                                                                                                                 | Type                                                                                                                                                                                                  | Required                                                                                                                                                                                              | Description                                                                                                                                                                                           |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `condition`                                                                                                                                                                                           | Record<string, Record<string, *string*>>                                                                                                                                                              | :heavy_minus_sign:                                                                                                                                                                                    | Optional condition for additional filtering (rare)                                                                                                                                                    |
| `notResources`                                                                                                                                                                                        | *string*[]                                                                                                                                                                                            | :heavy_minus_sign:                                                                                                                                                                                    | ARN patterns rendered as IAM `NotResource`, in place of `resources`. Its one use is a<br/>tag-on-create grant whose implied check AWS authorizes against no resource; the build<br/>refuses it anywhere else. |
| `resources`                                                                                                                                                                                           | *string*[]                                                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                                                    | Resource ARNs to bind to                                                                                                                                                                              |