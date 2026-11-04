.if defined(X)
bsd: ## bsd
	@true
.elif ${Y} == 1
.for f in a b
${f}:
.endfor
.endif
.include <bsd.prog.mk>
.MAIN: bsd
