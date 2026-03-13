# DataEmptyingBuckets

## Example Usage

```typescript
import { DataEmptyingBuckets } from "@alienplatform/platform-api/models";

let value: DataEmptyingBuckets = {
  bucketNames: [
    "<value 1>",
  ],
  type: "EmptyingBuckets",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `bucketNames`                         | *string*[]                            | :heavy_check_mark:                    | Names of the S3 buckets being emptied |
| `type`                                | *"EmptyingBuckets"*                   | :heavy_check_mark:                    | N/A                                   |