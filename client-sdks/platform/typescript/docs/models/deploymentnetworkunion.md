# DeploymentNetworkUnion


## Supported Types

### `models.DeploymentNetworkUseDefault`

```typescript
const value: models.DeploymentNetworkUseDefault = {
  type: "use-default",
};
```

### `models.DeploymentNetworkCreate`

```typescript
const value: models.DeploymentNetworkCreate = {
  type: "create",
};
```

### `models.DeploymentNetworkByoVpcAws`

```typescript
const value: models.DeploymentNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.DeploymentNetworkByoVpcGcp`

```typescript
const value: models.DeploymentNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.DeploymentNetworkByoVnetAzure`

```typescript
const value: models.DeploymentNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

