# 生成本地信任的 TLS 证书（推荐方式）
certs:
	@echo "=== Generating locally trusted certificates with mkcert ==="
	@mkdir -p fixtures/certs

	# 生成 wildcard 证书（支持 *.acme.com + localhost）
	@mkcert -cert-file fixtures/certs/wildcard.acme.com.crt \
	        -key-file  fixtures/certs/wildcard.acme.com.key \
	        *.acme.com acme.com localhost

	# 生成 api 专用证书（可选，如果你想单独使用）
	@mkcert -cert-file fixtures/certs/api.acme.com.crt \
	        -key-file  fixtures/certs/api.acme.com.key \
	        api.acme.com localhost

	@echo ""
	@echo "✅ Certificates generated successfully in fixtures/certs/"
	@echo "   • Wildcard: fixtures/certs/wildcard.acme.com.crt + .key"
	@echo "   • API:      fixtures/certs/api.acme.com.crt + .key"
	@echo "   • CA 已自动信任，无需再手动导入钥匙串"
