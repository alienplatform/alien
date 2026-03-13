# ReleaseAuthorFilterItem

## Example Usage

```typescript
import { ReleaseAuthorFilterItem } from "@alienplatform/platform-api/models";

let value: ReleaseAuthorFilterItem = {
  login: null,
  name: null,
  avatarUrl: "https://thorough-perp.com",
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `login`                                | *string*                               | :heavy_check_mark:                     | Provider username (e.g., GitHub login) |
| `name`                                 | *string*                               | :heavy_check_mark:                     | Git commit author name                 |
| `avatarUrl`                            | *string*                               | :heavy_check_mark:                     | Author avatar URL                      |