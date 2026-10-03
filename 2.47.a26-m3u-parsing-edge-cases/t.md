# A26: M3U parsing edge cases

Relative path starting with # is lost (src/m3u.rs:78-81,39); file://localhost/ not handled (:46); from_utf8_lossy breaks CP1251 lists (:67); Windows paths (:48-53); newline in a title breaks export (:77).

Fix each with a unit test (see E5).
