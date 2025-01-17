#ifndef GDK_CONTAINERS_HPP
#define GDK_CONTAINERS_HPP
#pragma once

#include <nlohmann/json.hpp>
#include <string>

namespace ga {
namespace sdk {

    // Rename from_key to to_key in the given JSON object
    bool json_rename_key(nlohmann::json& data, const std::string& from_key, const std::string& to_key);

    // Add a value to a JSON object if one is not already present under the given key
    template <typename T>
    T json_add_if_missing(nlohmann::json& data, const std::string& key, const T& value, bool or_null = false)
    {
        const auto p = data.find(key);
        if (p == data.end() || (or_null && p->is_null())) {
            data[key] = value;
            return value;
        }
        return *p;
    }

    // Set a value to a JSON object if it is non-default, otherwise remove any existing value.
    // This saves space storing the value if a default value is returned when its fetched.
    template <typename T = std::string>
    void json_add_non_default(
        nlohmann::json& data, const std::string& key, const T& value, const T& default_value = T())
    {
        const bool is_default = value == default_value;
        const auto p = data.find(key);
        const bool found = p != data.end();
        if (is_default) {
            if (found) {
                data.erase(p); // Remove existing value
            }
        } else {
            if (found) {
                *p = value; // Overwrite existing value
            } else {
                data[key] = value; // Insert new value
            }
        }
    }

    // Get a value if present and not null, otherwise return a default value
    template <typename T = std::string>
    T json_get_value(const nlohmann::json& data, const std::string& key, const T& default_value = T())
    {
        const auto p = data.find(key);
        if (p == data.end() || p->is_null()) {
            return default_value;
        }
        return *p;
    }
} // namespace sdk
} // namespace ga

#endif
