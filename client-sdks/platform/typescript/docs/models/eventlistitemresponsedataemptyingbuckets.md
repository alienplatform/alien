# EventListItemResponseDataEmptyingBuckets

## Example Usage

```typescript
import { EventListItemResponseDataEmptyingBuckets } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataEmptyingBuckets = {
  bucketNames: [],
  type: "EmptyingBuckets",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `bucketNames`                         | *string*[]                            | :heavy_check_mark:                    | Names of the S3 buckets being emptied |
| `type`                                | *"EmptyingBuckets"*                   | :heavy_check_mark:                    | N/A                                   |
