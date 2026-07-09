# Publishing Python SDK

Package: `sdk`
Version: `0.1.0`

`gnr8` never stores registry credentials and never uploads packages. Run these commands in this generated SDK directory after reviewing the generated files.

1. `python3 -m py_compile *.py`
2. `python3 -m build`
3. Upload with your own credentials, for example `python3 -m twine upload dist/*`.
