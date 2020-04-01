
#include "events.hpp"

namespace ga {
namespace sdk {
    namespace events {

        void emit_notification(annotated_mutex& mutex, GA_notification_handler* handler, void** context,
            std::string event, nlohmann::json details)
        {
            if (handler != nullptr && *handler != nullptr) {
                locker_t locker{ mutex };
                nlohmann::json* notify_details = new nlohmann::json({ { "event", event }, { event, details } });
                call_notification_handler(handler, context, locker, notify_details);
            }
        }

        void call_notification_handler(
            GA_notification_handler* handler, void** context, locker_t& locker, nlohmann::json* details)
        {
            GDK_RUNTIME_ASSERT(locker.owns_lock());
            GDK_RUNTIME_ASSERT(*handler != nullptr);
            // Note: notification recipient must destroy the passed JSON
            const auto details_c = reinterpret_cast<const GA_json*>(details);
            {
                unique_unlock unlocker(locker);
                (*handler)(*context, details_c);
            }
            if (details_c == nullptr) {
                *handler = nullptr;
                *context = nullptr;
            }
        }

    } // namespace events
} // namespace sdk
} // namespace ga
