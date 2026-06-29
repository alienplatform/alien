FROM nginx:alpine

COPY docker-entrypoint.d/10-nginx-resolver.envsh /docker-entrypoint.d/10-nginx-resolver.envsh
COPY nginx.conf.template /etc/nginx/templates/default.conf.template

# Expose port
EXPOSE 8080

# Start nginx
CMD ["nginx", "-g", "daemon off;"]

