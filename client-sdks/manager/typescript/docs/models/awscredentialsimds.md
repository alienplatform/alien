# AwsCredentialsImds

AWS Instance Metadata Service credentials.

## Example Usage

```typescript
import { AwsCredentialsImds } from "@alienplatform/manager-api/models";

let value: AwsCredentialsImds = {
  type: "imds",
};
```

## Fields

| Field                           | Type                            | Required                        | Description                     |
| ------------------------------- | ------------------------------- | ------------------------------- | ------------------------------- |
| `endpoint`                      | *string*                        | :heavy_minus_sign:              | Optional IMDS endpoint override |
| `type`                          | *"imds"*                        | :heavy_check_mark:              | N/A                             |