# Test HTTP Server Image

A minimalistic service for testing purposes that responds with "Hello from test Cloud Run!" on any request.

Works for Cloud Run and Azure Container Apps.


## GCP - Push to Artifact Registry

```bash
docker build --platform linux/amd64 -t test-cloudrun .
docker tag test-cloudrun:latest <region>-docker.pkg.dev/<project>/test-cloudrun-repo/test-cloudrun:latest
gcloud artifacts repositories create test-cloudrun-repo --repository-format=docker --location=<region> --project <project>
gcloud auth configure-docker <region>-docker.pkg.dev
docker push <region>-docker.pkg.dev/<project>/test-cloudrun-repo/test-cloudrun:latest
```

## Azure - Push to Container Registry

```bash
docker build --platform linux/amd64 -t test-containerapp .
az acr create --resource-group <resource-group> --name <acr-name> --sku Basic --location <region>
az acr login --name <acr-name>
docker tag test-containerapp:latest <acr-name>.azurecr.io/test-containerapp:latest
docker push <acr-name>.azurecr.io/test-containerapp:latest
``` 
