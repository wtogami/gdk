
#ifndef GA_EVENTS_H
#define GA_EVENTS_H

#include <nlohmann/json.hpp>

#include "include/gdk.h"
#include "threading.hpp"

namespace ga {
namespace sdk {
    namespace events {
        using locker_t = annotated_unique_lock<annotated_mutex>;

        void emit_notification(annotated_mutex& mutex, GA_notification_handler* handler, void** context,
            std::string event, nlohmann::json details);
        void call_notification_handler(
            GA_notification_handler* handler, void** context, locker_t& locker, nlohmann::json* details);
        void set_notification_handler(GA_notification_handler* from_handler, void** from_context,
            GA_notification_handler to_handler, void* to_context);

    } // namespace events
} // namespace sdk
} // namespace ga

#endif /* GA_EVENTS_H */
