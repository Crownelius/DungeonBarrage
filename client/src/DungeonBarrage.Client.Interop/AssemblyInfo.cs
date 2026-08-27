using System.Diagnostics.CodeAnalysis;
using System.Runtime.InteropServices;

// Restricts native loading to the assembly's own directory for every import in this assembly.
//
// This is belt-and-braces with NativeLibraryResolver, which already resolves an absolute path and
// never consults the OS search order. Both exist because this library *is* the game rules: a
// substituted `db_sim_ffi` is a full authoritative-logic replacement, not a cosmetic hijack. If a
// future import is ever added without going through the resolver, this attribute still denies the
// broad search path that would otherwise make it hijackable.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]

// CA5393 treats anything other than System32 as unsafe, because it is written for callers loading
// *operating system* libraries, where the OS directory is the trustworthy one. That premise does
// not hold here: `db_sim_ffi` is an application-owned library that ships beside this assembly and
// will never exist in System32, so naming System32 would simply guarantee a failed load.
//
// AssemblyDirectory is the narrowest correct value, and it is what CLIENT_SPEC 8.6 requires:
// application-owned absolute paths, never the working directory or the OS's broad search order. In
// practice the search path is not even reached — `NativeLibraryResolver` supplies an absolute path
// through `SetDllImportResolver`, which runs before any probing — so this attribute only bounds the
// fallback that a future unresolved import would otherwise get.
[assembly: SuppressMessage(
    "Security",
    "CA5393:Do not use unsafe DllImportSearchPath value",
    Justification =
        "AssemblyDirectory is the narrowest correct value for an application-owned native library; "
        + "System32 is inapplicable. Loads are resolved to absolute paths by NativeLibraryResolver "
        + "before any search path is consulted. See CLIENT_SPEC 8.6.")]
