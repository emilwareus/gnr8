// Package load wraps golang.org/x/tools/go/packages in full-type mode
// (LoadAllSyntax) so the caller gets ASTs, complete type info, and positions in a
// single pass (GO-01, RESEARCH Pattern 1).
//
// NeedDeps is the load-bearing mode bit: it makes imported packages (uuid, time,
// gin) fully typed rather than placeholder stubs, which is required to resolve
// uuid.UUID / time.Time identities (RESEARCH §1).
package load

import (
	"go/token"

	"golang.org/x/tools/go/packages"
)

// loadMode is the LoadAllSyntax bitset plus NeedModule.
// NeedName|NeedFiles|NeedImports|NeedDeps|NeedTypes|NeedTypesSizes|NeedSyntax|NeedTypesInfo
// is LoadAllSyntax; NeedModule additionally populates pkg.Module so the caller can
// identify the main module (used to scope extraction to the target's own packages).
const loadMode = packages.NeedName |
	packages.NeedFiles |
	packages.NeedImports |
	packages.NeedDeps |
	packages.NeedTypes |
	packages.NeedTypesSizes |
	packages.NeedSyntax |
	packages.NeedTypesInfo |
	packages.NeedModule

// LoadError is a structured per-package error the caller turns into a diagnostic
// (GO-06): packages.Load reports per-package errors instead of failing hard, so
// they must not be silently dropped (Pitfall 2).
type LoadError struct {
	// Pkg is the package path the error belongs to (may be empty).
	Pkg string
	// Pos is the source position "file:line:col" if go/packages provided one.
	Pos string
	// Msg is the underlying error text.
	Msg string
	// Kind names the loader stage that produced the error, taken from
	// packages.Error.Kind. Which stage failed decides what the reader has to fix,
	// and the loader already states it — dropping it left every failure reading as
	// one undifferentiated "load error".
	Kind string
}

// ErrorKind names the loader stage behind a packages.Error.
//
// The four values mirror packages.ErrorKind exactly, so the caller renders the
// loader's own classification rather than a gnr8 invention:
//
//   - "list"    the go command could not describe the package (module, build, or
//     toolchain resolution failed) — the environment, not the code;
//   - "parse"   a source file did not parse;
//   - "type"    type-checking rejected the package;
//   - "unknown" the loader did not classify it.
func ErrorKind(kind packages.ErrorKind) string {
	switch kind {
	case packages.ListError:
		return "list"
	case packages.ParseError:
		return "parse"
	case packages.TypeError:
		return "type"
	case packages.UnknownError:
		return "unknown"
	default:
		return "unknown"
	}
}

// Result bundles the loaded packages, the shared FileSet for span resolution, and
// any per-package errors collected during loading.
type Result struct {
	Packages []*packages.Package
	Fset     *token.FileSet
	Errors   []LoadError
}

// Load type-checks the module rooted at targetDir in full-type mode and returns
// the packages, a shared FileSet, and any per-package errors.
//
// When patterns is empty, it loads "./..." for the historical whole-module
// behavior. Callers may pass package patterns such as "./internal/foo/ports" to
// scope extraction in large modules that contain non-contract examples, tools,
// generated files, or fixtures.
//
// A non-nil error is returned only for hard loader failures (e.g. the directory
// is not a module); per-package type/parse errors are returned in Result.Errors
// for the caller to diagnose without aborting (GO-06).
func Load(targetDir string, patterns ...string) (*Result, error) {
	fset := token.NewFileSet()
	cfg := &packages.Config{
		Dir:   targetDir,
		Mode:  loadMode,
		Tests: false,
		Fset:  fset,
	}

	if len(patterns) == 0 {
		patterns = []string{"./..."}
	}
	pkgs, err := packages.Load(cfg, patterns...)
	if err != nil {
		return nil, err
	}

	var loadErrs []LoadError
	packages.Visit(pkgs, nil, func(pkg *packages.Package) {
		for _, e := range pkg.Errors {
			loadErrs = append(loadErrs, LoadError{
				Pkg:  pkg.PkgPath,
				Pos:  e.Pos,
				Msg:  e.Msg,
				Kind: ErrorKind(e.Kind),
			})
		}
	})

	return &Result{Packages: pkgs, Fset: fset, Errors: loadErrs}, nil
}
