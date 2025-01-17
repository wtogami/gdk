#ifndef GDK_GA_CACHE_HPP
#define GDK_GA_CACHE_HPP
#pragma once

#include "ga_wally.hpp"
#include "gsl_wrapper.hpp"
#include "sqlite3/sqlite3.h"
#include <boost/optional.hpp>
#include <nlohmann/json.hpp>

namespace std {
template <> struct default_delete<struct sqlite3> {
    void operator()(struct sqlite3* ptr) const { sqlite3_close(ptr); }
};
template <> struct default_delete<struct sqlite3_stmt> {
    void operator()(struct sqlite3_stmt* ptr) const { sqlite3_finalize(ptr); }
};
} // namespace std

namespace ga {
namespace sdk {
    class network_parameters;

    struct cache final {
        cache(const network_parameters& net_params, const std::string& network_name);

        bool has_liquid_output(byte_span_t txhash, const uint32_t vout);
        boost::optional<nlohmann::json> get_liquid_output(byte_span_t txhash, const uint32_t vout);
        void insert_liquid_output(byte_span_t txhash, const uint32_t vout, nlohmann::json& utxo);

        bool has_liquid_blinding_nonce(byte_span_t pubkey, byte_span_t script);
        boost::optional<std::vector<unsigned char>> get_liquid_blinding_nonce(byte_span_t pubkey, byte_span_t script);
        void insert_liquid_blinding_nonce(byte_span_t pubkey, byte_span_t script, byte_span_t nonce);

        boost::optional<std::vector<unsigned char>> get_key_value(const std::string& key);

        void upsert_key_value(const std::string& key, byte_span_t value);
        void clear_key_value(const std::string& key);

        void save_db(byte_span_t encryption_key);
        void load_db(byte_span_t encryption_key, const uint32_t type);

    private:
        uint32_t m_type = 0;
        bool m_require_write = false;
        const std::string m_network_name;
        const bool m_is_liquid;
        std::unique_ptr<sqlite3> m_db;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_blinding_nonce_has;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_blinding_nonce_search;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_blinding_nonce_insert;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_output_has;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_output_search;
        std::unique_ptr<sqlite3_stmt> m_stmt_liquid_output_insert;
        std::unique_ptr<sqlite3_stmt> m_stmt_key_value_upsert;
        std::unique_ptr<sqlite3_stmt> m_stmt_key_value_search;
        std::unique_ptr<sqlite3_stmt> m_stmt_key_value_delete;
    };

} // namespace sdk
} // namespace ga

#endif
