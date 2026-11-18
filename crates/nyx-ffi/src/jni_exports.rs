//! Exports JNI de `io.nyx.browser.NyxCore` (objet Kotlin, méthodes
//! `@JvmStatic external`). Marshalling uniquement — toute décision vit
//! dans les helpers de `lib.rs`, eux-mêmes délégués à `nyx-core`.
//!
//! `#[no_mangle]` est requis pour que la JVM trouve les symboles
//! `Java_<package>_<classe>_<méthode>` ; il déclenche le lint `unsafe_code`,
//! autorisé ICI SEULEMENT. Aucun bloc `unsafe {}` : l'API de la crate `jni`
//! est sûre. Chaque export est enveloppé dans `catch_unwind` — une panique
//! ne doit jamais traverser la frontière FFI (elle avorterait le process).
#![allow(unsafe_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JByteArray, JClass, JObjectArray, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

fn read_string(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(String::from).unwrap_or_default()
}

fn new_string(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|j| j.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn to_jboolean(b: bool) -> jboolean {
    if b { JNI_TRUE } else { JNI_FALSE }
}

fn read_byte_arrays(env: &mut JNIEnv, arrays: &JObjectArray) -> Vec<Vec<u8>> {
    if arrays.is_null() {
        return Vec::new();
    }
    let len = env.get_array_length(arrays).unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..len {
        let Ok(obj) = env.get_object_array_element(arrays, i) else { continue };
        if obj.is_null() {
            continue;
        }
        let bytes = JByteArray::from(obj);
        if let Ok(v) = env.convert_byte_array(&bytes) {
            out.push(v);
        }
        let _ = env.delete_local_ref(bytes);
    }
    out
}

/// `NyxCore.initFilters(lists: Array<ByteArray>): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_initFilters<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    gz_lists: JObjectArray<'local>,
) -> jboolean {
    catch_unwind(AssertUnwindSafe(|| {
        let lists = read_byte_arrays(&mut env, &gz_lists);
        to_jboolean(crate::init_filters(&lists))
    }))
    .unwrap_or(JNI_FALSE)
}

/// `NyxCore.filtersReady(): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_filtersReady<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jboolean {
    to_jboolean(crate::filters_ready())
}

/// `NyxCore.shouldBlock(url: String, sourceUrl: String, resourceType: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_shouldBlock<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
    source_url: JString<'local>,
    resource_type: JString<'local>,
) -> jboolean {
    catch_unwind(AssertUnwindSafe(|| {
        let url = read_string(&mut env, &url);
        let source = read_string(&mut env, &source_url);
        let rtype = read_string(&mut env, &resource_type);
        to_jboolean(crate::should_block(&url, &source, &rtype))
    }))
    .unwrap_or(JNI_FALSE)
}

/// `NyxCore.resourceType(url: String, accept: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_resourceType<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
    accept: JString<'local>,
) -> jstring {
    let rtype = catch_unwind(AssertUnwindSafe(|| {
        let url = read_string(&mut env, &url);
        let accept = read_string(&mut env, &accept);
        crate::resource_type(&url, &accept)
    }))
    .unwrap_or("other");
    new_string(&mut env, rtype)
}

/// `NyxCore.resolveInput(text: String): String` — `""` si refusé.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_resolveInput<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    text: JString<'local>,
) -> jstring {
    let resolved = catch_unwind(AssertUnwindSafe(|| {
        let text = read_string(&mut env, &text);
        crate::resolve_input(&text)
    }))
    .unwrap_or_default();
    new_string(&mut env, &resolved)
}

/// `NyxCore.isNavigable(url: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_isNavigable<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
) -> jboolean {
    catch_unwind(AssertUnwindSafe(|| {
        let url = read_string(&mut env, &url);
        to_jboolean(crate::is_navigable(&url))
    }))
    .unwrap_or(JNI_FALSE)
}

/// `NyxCore.renderPage(template: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxCore_renderPage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    template: JString<'local>,
) -> jstring {
    let page = catch_unwind(AssertUnwindSafe(|| {
        let template = read_string(&mut env, &template);
        crate::render_page(&template)
    }))
    .unwrap_or_default();
    new_string(&mut env, &page)
}
