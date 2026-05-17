use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

pub(crate) fn install_resize_transition_guard() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(root) = document.document_element() else {
        return;
    };

    let resize_timeout = Rc::new(RefCell::new(None::<(i32, Closure<dyn FnMut()>)>));
    let window_for_handler = window.clone();
    let root_for_handler = root.clone();
    let timeout_for_handler = Rc::clone(&resize_timeout);

    let handler = Closure::<dyn FnMut()>::new(move || {
        let _ = root_for_handler.class_list().add_1("is-window-resizing");

        if let Some((timeout_id, _callback)) = timeout_for_handler.borrow_mut().take() {
            window_for_handler.clear_timeout_with_handle(timeout_id);
        }

        let root_for_timeout = root_for_handler.clone();
        let timeout_for_timeout = Rc::clone(&timeout_for_handler);
        let timeout_callback = Closure::<dyn FnMut()>::new(move || {
            let _ = root_for_timeout.class_list().remove_1("is-window-resizing");
            timeout_for_timeout.borrow_mut().take();
        });

        if let Ok(timeout_id) = window_for_handler
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_callback.as_ref().unchecked_ref(),
                120,
            )
        {
            timeout_for_handler
                .borrow_mut()
                .replace((timeout_id, timeout_callback));
        }
    });
    let callback = handler.as_ref().unchecked_ref::<js_sys::Function>();

    if window
        .add_event_listener_with_callback("resize", callback)
        .is_ok()
    {
        handler.forget();
    }
}
