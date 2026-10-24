# LegacyStore

A bundle store from before the current one. Installations whose setup granted it read every
release's bundle key from it, and build from the base image repositories that setup granted.

## Example Usage

```typescript
import { LegacyStore } from "@alienplatform/platform-api/models";

let value: LegacyStore = {
  uriTemplate: "<value>",
};
```

## Fields

| Field                                                                                                                                                                  | Type                                                                                                                                                                   | Required                                                                                                                                                               | Description                                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `repositoryRenames`                                                                                                                                                    | [models.RepositoryRename](../models/repositoryrename.md)[]                                                                                                             | :heavy_minus_sign:                                                                                                                                                     | How this store's bundles name a private base image: the first rule whose `from` prefixes<br/>the repository replaces that prefix with `to`. No match keeps the repository. |
| `uriTemplate`                                                                                                                                                          | *string*                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                     | `s3://` bucket template with exactly one `{region}` token.                                                                                                             |