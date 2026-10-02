# CreateReleaseChannelRequest

Body of `POST /v1/release-channels`.

## Example Usage

```typescript
import { CreateReleaseChannelRequest } from "@alienplatform/manager-api/models";

let value: CreateReleaseChannelRequest = {
  name: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `name`                                                         | *string*                                                       | :heavy_check_mark:                                             | Lowercase letters, digits and hyphens, starting with a letter. |
| `releaseId`                                                    | *string*                                                       | :heavy_minus_sign:                                             | Release the channel starts at.                                 |